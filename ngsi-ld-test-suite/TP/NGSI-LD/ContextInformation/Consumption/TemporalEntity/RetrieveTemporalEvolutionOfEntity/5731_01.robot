*** Settings ***
Documentation       Verify the 5.7.3.4 Linked Entity projection rejection,
...                 an edge the official 020 TPs skip.
...
...                 5.7.3.4: "If projection attributes are present and
...                 indicate the use of Linked Entity retrieval, an error of
...                 type BadRequestData shall be raised" — unconditional:
...                 the temporal retrieve defines no join parameter.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Test Cases ***
5731_01_01 Temporal Retrieve With Linked Projection Is BadRequestData
    [Documentation]    5.7.3.4: pick using the {…} Linked Entity selection on
    ...    the temporal resource → 400 BadRequestData.
    [Tags]    e-retrieve-temporal    5_7_3    since_v1.9.1
    &{headers}=    Create Dictionary    Accept=application/ld+json
    ${response}=    GET
    ...    url=${temporal_api_url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}/urn:ngsi-ld:Building:5731?pick=owner{name}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_BAD_REQUEST_DATA}
