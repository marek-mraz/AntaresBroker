*** Settings ***
Documentation       Verify that, when one has an inclusive registration on a Context Broker, entities with same id on
...                 the Context Broker and on a Context Source, if one queries the Context Broker the query gets
...                 forwarded to the Context Source correctly

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.json
${entity_payload_filename2}             vehicle-simple-different-attributes.jsonld
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-complete.jsonld


*** Test Cases ***
D011_01_05_inc Query The Context Broker With Type
    [Documentation]    Check that entities on the Context Broker and Context Source with the same id get merged and
    ...    returned as one entity
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_7_2
    ${serialized_entity}=    Load Entity As Serialized Array    ${entity_payload_filename}    ${entity_id}
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=Vehicle    200    ${serialized_entity}

    ${response}=    Query Entities    entity_types=Vehicle    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    @{entities_id}=    Create List    ${entity_id}
    Check Response Body Containing Entities URIS set to    ${entities_id}    ${response.json()}
    Should Have Value In Json    ${response.json()[0]}    $.brandName
    Should Have Value In Json    ${response.json()[0]}    $.speed
    Should Have Value In Json    ${response.json()[0]}    $.isParked
    Should Have Value In Json    ${response.json()[0]}    $.isParked2


*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${response}=    Create Entity    ${entity_payload_filename2}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}
    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
