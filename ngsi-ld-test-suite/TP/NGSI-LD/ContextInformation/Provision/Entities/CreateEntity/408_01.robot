*** Settings ***
Documentation       Verify the 4.8 Temporal Property value space on entity creation.
...
...                 Clause 4.8: "Temporal Properties in NGSI-LD shall be represented based
...                 on the DateTime data type as mandated by clause 4.6.3" — and a
...                 TemporalProperty is non-reified, represented only by its Value.
...                 Clause 4.6.3: "The trailing timestamp component shall always be equal
...                 to the character Z. Therefore, all timestamps shall be expressed in
...                 UTC" — a numeric offset is not an alternative form.
...
...                 Antares extension TP — no official TP is tagged 4_8; the temporal
...                 value-space long tail (offset instead of Z, reified observedAt) is
...                 exactly the class the official set skips.

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       Create Entity With Invalid Temporal Property


*** Test Cases ***    FILENAME
408_01_01 ObservedAt Is Not A DateTime
    [Tags]    e-create    4_8    4_6_3    since_v1.9.1
    temporal-observedat-not-datetime.jsonld
408_01_02 ObservedAt Carries An Offset Instead Of Z
    [Tags]    e-create    4_8    4_6_3    since_v1.9.1
    temporal-observedat-offset.jsonld
408_01_03 ObservedAt Is Reified
    [Tags]    e-create    4_8    since_v1.9.1
    temporal-observedat-reified.jsonld


*** Keywords ***
Create Entity With Invalid Temporal Property
    [Documentation]    4.8/4.6.3: a Temporal Property whose value is not a UTC DateTime
    ...    string (non-DateTime, numeric offset, or a reified object) is invalid content
    ...    and creation shall fail with BadRequestData
    [Arguments]    ${filename}
    ${response}=    Create Entity From File    ${filename}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
