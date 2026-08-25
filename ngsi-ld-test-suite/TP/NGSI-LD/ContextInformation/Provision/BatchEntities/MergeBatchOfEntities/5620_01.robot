*** Settings ***
Documentation       Verify 5.6.20.4 Batch Entity Merge validation and output
...                 edge cases the official 057 TPs skip.
...
...                 5.6.20.4: "Execute the behaviour defined in clause 5.5.4
...                 on JSON-LD validation" — an input Array that is empty or
...                 contains a null item fails whole with BadRequestData.
...                 5.6.20.4 executes clause 5.6.17 per entity, so merging an
...                 Attribute whose value is "urn:ngsi-ld:null" deletes it;
...                 when all Entities merge the output is none (204, empty
...                 body) per 5.6.20.5.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${entity_payload_filename}=     building-simple-attributes.jsonld


*** Test Cases ***
5620_01_01 Null Item In Batch Merge Array Is BadRequestData
    [Documentation]    5.6.20.4 via 5.5.4: a null value in any item of the
    ...    input Array raises BadRequestData for the whole request.
    [Tags]    be-merge    5_6_20    since_v1.9.1
    ${response}=    Batch Request Entities From File    merge    filename=batch/null-item.jsonld
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_BAD_REQUEST_DATA}

5620_01_02 Empty Batch Merge Array Is BadRequestData
    [Documentation]    5.6.20.4 via 5.5.4: an empty input Array raises
    ...    BadRequestData.
    [Tags]    be-merge    5_6_20    since_v1.9.1
    ${response}=    Batch Request Entities From File    merge    filename=batch/empty.jsonld
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_BAD_REQUEST_DATA}

5620_01_03 Merge Null Deletes The Attribute
    [Documentation]    5.6.20.4 executes clause 5.6.17 per entity: an
    ...    Attribute merged with the value "urn:ngsi-ld:null" is deleted,
    ...    other Attributes survive, and the all-success output is 204 with
    ...    an empty body (5.6.20.5). The deleted Attribute and the null
    ...    sentinel must NOT appear in the retrieved entity.
    [Tags]    be-merge    5_6_20    since_v1.9.1
    ${entity_id}=    Generate Random Building Entity Id
    ${create_response}=    Create Entity    ${entity_payload_filename}    ${entity_id}
    Check Response Status Code    201    ${create_response.status_code}
    ${fragment}=    Evaluate
    ...    {"id": "${entity_id}", "type": "Building", "subCategory": "urn:ngsi-ld:null", "@context": ["${ngsild_test_suite_context}"]}
    @{entities_to_be_merged}=    Create List    ${fragment}
    ${response}=    Batch Merge Entities    @{entities_to_be_merged}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    subCategory
    Should Not Contain    ${response.text}    urn:ngsi-ld:null
    Should Contain    ${response.text}    airQualityLevel
    [Teardown]    Delete Entity    ${entity_id}
