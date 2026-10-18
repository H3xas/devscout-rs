interface AnchorButtonProps {
  label: string;
}

const AnchorButton = ({ label }: AnchorButtonProps) => {
  return <button>{label}</button>;
};

export default AnchorButton;
