*** Settings ***
Documentation       Verify that, when a client queries a Context Broker via GET or POST with an inclusive
...                 registration, the broker forwards the request to the Context Source using the operation allowed by
...                 the registration (queryEntity or queryBatch)

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Teardown       Delete Registration And Stop Context Source Mock Server
Test Template       Setup Registration And Context Source Mock Server And Query The Context Broker With Type


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-complete.jsonld


*** Test Cases ***
D011_03_inc_01 Query The Context Broker Via GET Using queryEntity
    [Documentation]    Check that if one queries the Context Broker for type, and the registration only allows the
    ...    queryEntity operation, the request is forwarded as expected
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_7_2
    queryEntity    GET    /ngsi-ld/v1/entities
D011_03_inc_02 Query The Context Broker Via POST Using queryBatch
    [Documentation]    Check that if one queries the Context Broker for type, and the registration only allows the
    ...    queryBatch operation, the request is forwarded as expected
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_7_2
    queryBatch    POST    /ngsi-ld/v1/entityOperations/query


*** Keywords ***
Setup Registration And Context Source Mock Server And Query The Context Broker With Type
    [Documentation]    Check that if one queries the Context Broker for type, and the registration only allows the
    ...    requested operation, the request is forwarded as expected
    [Arguments]    ${operation}    ${method}    ${url}
    ${entity_id}=    Generate Random Vehicle Entity Id

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${operations}=    Create List    ${operation}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    operations=${operations}
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}
    Start Context Source Mock Server

    ${serialized_entity}=    Load Entity As Serialized Array    ${entity_payload_filename}    ${entity_id}

    @{entities_id}=    Create List    ${entity_id}
    IF    '${method}' == 'GET'
        Set Stub Reply    ${method}    ${url}?id=${entity_id}&type=Vehicle    200    ${serialized_entity}
        ${response}=    Query Entities
        ...    entity_ids=${entities_id}
        ...    entity_types=Vehicle
        ...    context=${ngsild_test_suite_context}
    ELSE IF    '${method}' == 'POST'
        Set Stub Reply    ${method}    ${url}    200    ${serialized_entity}
        ${entity_selector}=    Create Dictionary    type=Vehicle    id=${entity_id}
        @{entities}=    Create List    ${entity_selector}
        ${response}=    Query Entities Via POST
        ...    entities=${entities}
        ...    content_type=${CONTENT_TYPE_LD_JSON}
        ...    accept=${CONTENT_TYPE_JSON}
        ...    context=${ngsild_test_suite_context}
    END

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Entities URIS set to    ${entities_id}    ${response.json()}

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
