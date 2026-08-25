*** Settings ***
Documentation       Verify the 5.7.10 error path and what the 5.7.8 attribute
...                 list must never contain.
...
...                 5.7.10 describes attributes "that belong to entity
...                 instances existing within the NGSI-LD system"; a name no
...                 entity carries is a ResourceNotFound (Table 6.3.2-1).
...                 The Entity members of 4.5.1 (id, type, scope) and the
...                 system temporal attributes of 6.3.11 (createdAt,
...                 modifiedAt, deletedAt) are not Attributes, so they belong
...                 to no attribute list.
...
...                 Antares extension TP — the official 025/026/027 TPs only
...                 exercise the happy path.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entity
Suite Teardown      Delete Initial Entity


*** Variables ***
${filename}=    building-simple-attributes.json


*** Test Cases ***
5710_01_01 An Attribute No Entity Carries Is ResourceNotFound
    [Documentation]    5.7.10: only attributes of existing entity instances
    ...    are described; anything else is 404 ResourceNotFound.
    [Tags]    ed-attr    5_7_10    since_v1.9.1
    ${attr}=    Evaluate    'noSuchAttribute' + str(random.randint(10000, 99999))    modules=random
    ${response}=    Retrieve Attribute    attribute_name=${attr}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}

5710_01_02 Entity Members Are Not Discoverable Attributes
    [Documentation]    4.5.1 / 6.3.11: id, type, scope and the system
    ...    temporal attributes are Entity members, not Attributes — they are
    ...    absent from the 5.7.8 attribute list and unknown to 5.7.10.
    [Tags]    ed-attr    ed-attrs    5_7_8    5_7_10    since_v1.9.1
    ${response}=    Retrieve Attributes    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${attributes}=    Evaluate    $response.json()['attributeList']
    FOR    ${member}    IN    id    type    scope    createdAt    modifiedAt    deletedAt
        Should Not Contain    ${attributes}    ${member}
        ${response}=    Retrieve Attribute
        ...    attribute_name=${member}
        ...    context=${ngsild_test_suite_context}
        Check Response Status Code    404    ${response.status_code}
    END

5710_01_03 An Undefined Query Parameter Is InvalidRequest
    [Documentation]    6.3.20: GET /attributes defines details, local and
    ...    count, GET /attributes/{attrId} only local; any other query
    ...    parameter is rejected with 400 InvalidRequest.
    [Tags]    ed-attr    ed-attrs    5_7_8    5_7_10    6_3_20    since_v1.9.1
    FOR    ${path}    IN
    ...    ${ATTRIBUTES_ENDPOINT_PATH}?limit=5
    ...    ${ATTRIBUTES_ENDPOINT_PATH}/temperature?details=true
        ${response}=    GET    url=${url}/${path}    expected_status=any
        Check Response Status Code    400    ${response.status_code}
        Check Response Body Containing ProblemDetails Element Containing Type Element set to
        ...    ${response.json()}
        ...    ${ERROR_TYPE_INVALID_REQUEST}
    END


*** Keywords ***
Setup Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${response.status_code}
    Set Suite Variable    ${entity_id}

Delete Initial Entity
    Delete Entity    ${entity_id}
