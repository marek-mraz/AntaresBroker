*** Settings ***
Documentation       Verify the 4.5.2.2 "ngsildproof" member: "a Property with
...                 the non-reified subproperties \"entityIdSealed\" and
...                 \"entityTypeSealed\" as specified in [35]. The value of its
...                 \"value\" element shall be an object containing the W3C
...                 Data integrity \"proof\" structure [35]." (See clause C.11
...                 for the example this fixture mirrors.) Annex B maps
...                 entityIdSealed as a plain term and entityTypeSealed with
...                 "@type": "@vocab", so the sealed type compacts back to the
...                 short type name. 4.5.2.2 also prohibits the sealed members
...                 on any other attribute.
...
...                 Antares extension TP — the suite contains zero ngsildproof
...                 coverage.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Entity    ${entity_id}


*** Test Cases ***
452_03_01 Ngsildproof With Sealed Members Round-Trips
    [Documentation]    4.5.2.2/C.11: the proof Property round-trips with its
    ...    W3C proof value object untouched and BOTH non-reified sealed
    ...    subproperties present as bare strings — entityTypeSealed compacted
    ...    back to the short type name per the annex-B @vocab coercion.
    [Tags]    e-create    e-retrieve    4_5_2_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "speed": {"type": "Property", "value": 55}, "ngsildproof": {"type": "Property", "entityIdSealed": $entity_id, "entityTypeSealed": "Vehicle", "value": {"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnNHc6SaVWoT", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "zQeVbY4oey5q2M3XKaxup3tmzN4DRFTLVqpLMweBrSxMY"}}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}

    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Set Variable    ${response.json()}
    ${proof_type}=    Evaluate    $body['ngsildproof']['type']
    Should Be Equal    ${proof_type}    Property
    ${sealed_id}=    Evaluate    $body['ngsildproof']['entityIdSealed']
    Should Be Equal    ${sealed_id}    ${entity_id}
    ${sealed_type}=    Evaluate    $body['ngsildproof']['entityTypeSealed']
    Should Be Equal    ${sealed_type}    Vehicle
    ${proof_value}=    Evaluate    $body['ngsildproof']['value']['proofValue']
    Should Be Equal    ${proof_value}    zQeVbY4oey5q2M3XKaxup3tmzN4DRFTLVqpLMweBrSxMY
    ${cryptosuite}=    Evaluate    $body['ngsildproof']['value']['cryptosuite']
    Should Be Equal    ${cryptosuite}    eddsa-rdfc-2022
    # non-reified: the sealed members are BARE strings, never Property objects
    ${sealed_is_str}=    Evaluate    isinstance($body['ngsildproof']['entityIdSealed'], str) and isinstance($body['ngsildproof']['entityTypeSealed'], str)
    Should Be True    ${sealed_is_str}

452_03_02 Sealed Members Outside Ngsildproof Are Rejected
    [Documentation]    4.5.2.2 prohibitions: "entityIdSealed and
    ...    entityTypeSealed shall never be present, unless the Property name
    ...    is ngsildproof" — 400 BadRequestData, and nothing is created.
    [Tags]    e-create    4_5_2_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "speed": {"type": "Property", "value": 1, "entityIdSealed": $entity_id}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to    ${response.json()}    https://uri.etsi.org/ngsi-ld/errors/BadRequestData
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}

452_03_03 A Non-Object Proof Value Is Rejected
    [Documentation]    4.5.2.2: "The value of its \"value\" element shall be
    ...    an object containing the W3C Data integrity \"proof\" structure" —
    ...    a string proof value is 400 BadRequestData; the same holds for
    ...    non-string sealed members.
    [Tags]    e-create    4_5_2_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "ngsildproof": {"type": "Property", "value": "not-a-proof-object"}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}

    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "ngsildproof": {"type": "Property", "value": {"type": "DataIntegrityProof"}, "entityTypeSealed": 42}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}
